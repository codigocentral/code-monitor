#!/usr/bin/env bash
# All service, account, crypto and filesystem effects below are doubles.
set -u
TASK_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FIXTURE_ROOT="$(/usr/bin/mktemp -d "${TMPDIR%/}/monitor-unit.XXXXXX")"
/bin/mkdir -p "$FIXTURE_ROOT/certs" "$FIXTURE_ROOT/default/certs"
export FIXTURE_ROOT
PLAN=0 FAILURES=0
record() { printf '%s\n' "$*" >> "$FIXTURE_ROOT/calls"; }
check() {
  PLAN=$((PLAN+1))
  if "$@"; then printf 'ok %s - %s\n' "$PLAN" "$*"; else printf 'not ok %s - %s\n' "$PLAN" "$*"; FAILURES=$((FAILURES+1)); fi
}
has_call() { /usr/bin/grep -q -- "$1" "$FIXTURE_ROOT/calls"; }
no_call() { ! has_call "$1"; }
output() { /usr/bin/grep -q -- "$1" "$FIXTURE_ROOT/stdout"; }
run_case() {
  local script="$1" expected="$2";shift 2
  : > "$FIXTURE_ROOT/calls"
  (
    set +u
    set -f
    builtin cd "$FIXTURE_ROOT"
    export CODE_MONITOR_PREFIX="$FIXTURE_ROOT/prefix"
    /bin/mkdir -p "$CODE_MONITOR_PREFIX/etc/code-monitor" "$CODE_MONITOR_PREFIX/etc/systemd/system" "$CODE_MONITOR_PREFIX/usr/local/bin" "$CODE_MONITOR_PREFIX/usr/local/etc/code-monitor" "$CODE_MONITOR_PREFIX/Library/LaunchDaemons" "$CODE_MONITOR_PREFIX/tmp"
    printf 'access_token = "%s"\n%s\n' "${UNIT_TOKEN-unit-token}" "${MANAGED_BLOCK:-}" > "$CODE_MONITOR_PREFIX/etc/code-monitor/config.toml"
    printf '#!/bin/bash\nprintf "binary double %%s\\n" "$*" >> "$FIXTURE_ROOT/calls"\nexit "${BINARY_FAIL:-0}"\n' > "$CODE_MONITOR_PREFIX/usr/local/bin/monitor-server"
    /bin/chmod +x "$CODE_MONITOR_PREFIX/usr/local/bin/monitor-server"
    pwd() { printf '%s\n' "$FIXTURE_ROOT"; }
    cd() { builtin cd "$FIXTURE_ROOT"; }
    mkdir() { record "mkdir $*"; }
    cp() { record "cp $*"; }
    rm() { record "rm $*"; }
    chmod() { record "chmod $*"; }
    id() { record "id $*"; if builtin [ "${1:-}" = -u ] && builtin [ "$#" = 1 ]; then printf '%s\n' "${UNIT_UID:-0}"; else return "${USER_MISSING:-0}"; fi; }
    useradd() { record "useradd $*"; }
    usermod() { record "usermod $*"; }
    chown() { record "chown $*"; }
    install() { record "install $*"; }
    sleep() { record "sleep $*"; }
    systemctl() { record "systemctl $*"; if builtin [ "${1:-}" = is-active ]; then return "${SERVICE_FAIL:-0}"; fi; }
    getent() { record "getent $*"; return "${GROUP_MISSING:-0}"; }
    uname() { if builtin [ "${1:-}" = -s ]; then printf '%s\n' "${UNIT_OS:-Linux}"; else printf '%s\n' "${UNIT_ARCH:-x86_64}"; fi; }
    command() { if builtin [ "${1:-}" = -v ]; then builtin [ "${2:-}" != "${MISSING_COMMAND:-}" ]; else builtin command "$@"; fi; }
    mktemp() { if builtin [ "${1:-}" = -d ]; then /usr/bin/mktemp -d "$FIXTURE_ROOT/temp.XXXXXX"; else /usr/bin/mktemp "$FIXTURE_ROOT/temp.XXXXXX"; fi; }
    curl() { record "curl $*"; if builtin [ "${NO_RELEASE:-0}" != 1 ]; then printf '{"tag_name":"v0.unit"}\n'; fi; }
    tar() { record "tar $*"; }
    ufw() { record "ufw $*"; if builtin [ "${1:-}" = status ]; then printf 'Status: %s\n' "${UFW_STATUS:-active}"; else return "${UFW_FAIL:-0}"; fi; }
    pg_lsclusters() { if builtin [ "${NO_CLUSTERS:-0}" != 1 ]; then printf '16 main 5432 online postgres /unit /unit\n16 stopped 5433 down postgres /unit /unit\n'; fi; }
    docker() {
      record "docker $*"
      if builtin [ "${1:-}" = inspect ]; then
        if builtin [ "$#" = 2 ]; then return "${DOCKER_MISSING:-0}";
        elif builtin [ "${4:-}" = '{{range .Config.Env}}{{println .}}{{end}}' ]; then if builtin [ "${NO_ROOT_PASSWORD:-0}" != 1 ]; then printf 'MYSQL_ROOT_PASSWORD=unit-password\n'; fi
        else if builtin [ "${NO_CONTAINER_IP:-0}" != 1 ]; then printf '192.0.2.7\n'; fi;fi
      elif builtin [ "${2:-}" = -i ]; then /bin/cat > "$FIXTURE_ROOT/sql";
      else return "${MARIADB_CLIENT_MISSING:-0}";fi
    }
    sed() {
      if builtin [ "${1:-}" = -i ]; then record "sed $*";
      elif [[ "${2:-}" == *MYSQL_ROOT_PASSWORD* ]]; then
        # GNU sed's BRE alternation is not supported by macOS sed. This
        # collaborator double supplies the Linux command's documented result.
        local line
        while IFS= read -r line; do case "$line" in MYSQL_ROOT_PASSWORD=*|MARIADB_ROOT_PASSWORD=*) printf '%s\n' "${line#*=}";; esac; done
      elif [[ "${1:-}" == 's/.*=\s*'* ]]; then
        local line
        IFS= read -r line
        line="${line#*\"}"
        printf '%s\n' "${line%%\"*}"
      else /usr/bin/sed "$@";fi
    }
    head() { if builtin [ "${1:-}" = -c18 ]; then printf 'unit-random-bytes'; else /usr/bin/head "$@";fi; }
    sudo() {
      record "sudo $*"
      if builtin [ "${1:-}" = tee ]; then /bin/cat > "$FIXTURE_ROOT/generated-config";
      elif builtin [ "${1:-}" = -u ]; then /bin/cat > "$FIXTURE_ROOT/postgres.sql"; fi
      return "${SUDO_FAIL:-0}"
    }
    launchctl() { record "launchctl $*"; return "${LAUNCH_FAIL:-0}"; }
    openssl() { record "openssl $*"; if builtin [ "${1:-}" = rand ]; then printf "unit-generated-token\n"; fi; return "${OPENSSL_FAIL:-0}"; }
    [() {
      if builtin [ "${1:-}" = "!" ]; then
        shift
        if [ "$@"; then return 1; else return 0; fi
      elif builtin [ "${1:-}" = "$EUID" ] && builtin [ "${2:-}" = -ne ]; then
        builtin [ "${UNIT_UID:-0}" -ne "$3" ]
      elif builtin [ "${1:-}" = -f ]; then
        case "$2" in
          */target/release/*) builtin [ "${BUILD_KIND:-release}" = release ];;
          */target/debug/*) builtin [ "${BUILD_KIND:-release}" = debug ];;
          */config.toml) builtin [ "${CONFIG_EXISTS:-0}" = 1 ];;
          *.plist) builtin [ "${PLIST_EXISTS:-0}" = 1 ];;
          */tmp/monitor-server) builtin [ "${BINARY_EXISTS:-1}" = 1 ];;
          *) return 1;;
        esac
      else builtin [ "$@";fi
    }
    if builtin [ "$script" = scripts/publish-to-github.sh ]; then
      builtin cd "$FIXTURE_ROOT"
      /bin/rm -f .publishignore
      if builtin [ "${IGNORE_EXISTS:-1}" = 1 ]; then printf '# comment\n internal.md # trim\n\n' > .publishignore; fi
      git() {
        record "git $*"
        case "${1:-}" in
          rev-parse) printf '%s\n' "$FIXTURE_ROOT";;
          status) printf '%s' "${GIT_DIRTY:-}";;
          clone) return "${CLONE_FAIL:-0}";;
          -C) if builtin [ "${3:-}" = diff ] && builtin [ "${5:-}" = --quiet ]; then return "${GIT_DIFF:-1}"; fi;;
        esac
      }
      mktemp() {
        local temp_path
        temp_path="$(/usr/bin/mktemp -d "${TMPDIR%/}/monitor-publish.XXXXXX")"
        printf '%s\n' "${SCAN_CONTENT:-safe fixture}" > "$temp_path/README.md"
        printf '%s\n' "$temp_path"
      }
      tar() { /bin/cat >/dev/null; record "tar $*"; }
      find() { if builtin [ "${2:-}" = -path ]; then printf '%s/internal.md\n' "$1"; fi; }
    fi
    source "$TASK_ROOT/$script" "$@"
  ) > "$FIXTURE_ROOT/stdout" 2> "$FIXTURE_ROOT/stderr"
  local actual=$?
  check test "$actual" = "$expected"
  if builtin [ "$actual" != "$expected" ]; then /bin/cat "$FIXTURE_ROOT/stdout" "$FIXTURE_ROOT/stderr" "$FIXTURE_ROOT/calls"; fi
}
run_case generate-certs.sh 0 "$FIXTURE_ROOT/certs"
check has_call 'openssl req -x509 .* -days 3650'
check has_call 'openssl x509 .* -days 365 -sha256 -extfile .*server.ext'
check has_call 'chmod 600 .*server.key .*client.key'
check /usr/bin/grep -q 'IP.2 = ::1' "$FIXTURE_ROOT/certs/server.ext"
check /usr/bin/grep -q 'extendedKeyUsage = clientAuth' "$FIXTURE_ROOT/certs/client.ext"
OPENSSL_FAIL=7 run_case generate-certs.sh 7 "$FIXTURE_ROOT/certs"
check no_call chmod
run_case service/linux/install.sh 0
check has_call 'sudo systemctl enable code-monitor-server.service'
check has_call 'sudo chown -R code-monitor:code-monitor'
check /usr/bin/grep -q 'enable_authentication = true' "$FIXTURE_ROOT/generated-config"
USER_MISSING=1 CONFIG_EXISTS=1 BUILD_KIND=debug run_case service/linux/install.sh 0
check has_call 'sudo useradd --system --no-create-home'
check has_call '/target/debug/monitor-server'
check no_call 'sudo tee'
BUILD_KIND=none run_case service/linux/install.sh 1
check no_call sudo
SUDO_FAIL=8 run_case service/linux/install.sh 8
check no_call 'sudo systemctl'
run_case service/macos/install.sh 0
check has_call 'launchctl load .*com.codemonitor.server.plist'
check has_call 'sudo tee /usr/local/etc/code-monitor/config.toml'
CONFIG_EXISTS=1 BUILD_KIND=debug run_case service/macos/install.sh 0
check no_call 'sudo tee'
check has_call '/target/debug/monitor-client'
BUILD_KIND=none run_case service/macos/install.sh 1
check no_call sudo
LAUNCH_FAIL=9 run_case service/macos/install.sh 9
check has_call 'launchctl load'
PLIST_EXISTS=1 LAUNCH_FAIL=9 run_case service/macos/uninstall.sh 0
check has_call 'rm .*com.codemonitor.server.plist'
PLIST_EXISTS=0 run_case service/macos/uninstall.sh 0
check no_call launchctl
check has_call 'sudo rm -f /usr/local/bin/monitor-server'
run_case service/linux/uninstall.sh 0
check has_call 'sudo systemctl daemon-reload'
check has_call 'sudo rm -f /etc/systemd/system/code-monitor-server.service'
run_case scripts/publish-to-github.sh 2 --invalid
IGNORE_EXISTS=0 run_case scripts/publish-to-github.sh 1
GIT_DIRTY=' M fixture' run_case scripts/publish-to-github.sh 1
check no_call 'git clone'
run_case scripts/publish-to-github.sh 0
check has_call 'git clone'
check has_call 'git -C .* commit --quiet'
check no_call 'push origin'
check output 'Simulação concluída'
CLONE_FAIL=1 run_case scripts/publish-to-github.sh 0 --push
check has_call 'git init --quiet --initial-branch=main'
check has_call 'push origin main'
GIT_DIFF=0 run_case scripts/publish-to-github.sh 0
check no_call 'commit --quiet'
check output 'Nada mudou'
SCAN_CONTENT='10.10.0.9' run_case scripts/publish-to-github.sh 1
check no_call 'git clone'
# Remote provisioning: privileged commands are recorded, never executed.
UNIT_UID=501 run_case scripts/install-remote.sh 1 192.0.2.5
run_case scripts/install-remote.sh 1 0.0.0.0
BINARY_EXISTS=0 run_case scripts/install-remote.sh 1 192.0.2.5
BINARY_FAIL=6 run_case scripts/install-remote.sh 1 192.0.2.5
USER_MISSING=1 SUDO_USER=unit run_case scripts/install-remote.sh 0 192.0.2.5 50123 192.0.2.0/24
check has_call 'useradd --system'
check has_call 'ufw allow from 192.0.2.0/24 to any port 50123'
check /usr/bin/grep -q -- '--no-health' "$FIXTURE_ROOT/prefix/etc/systemd/system/code-monitor-server.service"
check /usr/bin/grep -q 'unit-generated-token' "$FIXTURE_ROOT/prefix/tmp/cm-token.txt"
CONFIG_EXISTS=1 GROUP_MISSING=1 UFW_STATUS=inactive run_case scripts/install-remote.sh 0 192.0.2.5
check no_call usermod
check output 'keeping existing token'
UFW_FAIL=1 run_case scripts/install-remote.sh 0 192.0.2.5
check output 'rule may already exist'
SERVICE_FAIL=1 run_case scripts/install-remote.sh 1 192.0.2.5
# Database provisioning boundaries: only synthetic facts and command doubles.
UNIT_UID=501 run_case scripts/enable-postgres.sh 1
CONFIG_EXISTS=0 run_case scripts/enable-postgres.sh 1
CONFIG_EXISTS=1 NO_CLUSTERS=1 run_case scripts/enable-postgres.sh 0
CONFIG_EXISTS=1 run_case scripts/enable-postgres.sh 0
check has_call 'sudo -u postgres psql -p 5432 -v ON_ERROR_STOP=1'
check /usr/bin/grep -q 'GRANT pg_monitor' "$FIXTURE_ROOT/postgres.sql"
check /usr/bin/grep -q 'CREATE ROLE "code-monitor" LOGIN' "$FIXTURE_ROOT/postgres.sql"
check has_call 'install -o code-monitor -g code-monitor -m 0640'
CONFIG_EXISTS=1 SERVICE_FAIL=1 run_case scripts/enable-postgres.sh 1
UNIT_UID=501 run_case scripts/enable-mariadb.sh 1
CONFIG_EXISTS=0 run_case scripts/enable-mariadb.sh 1
CONFIG_EXISTS=1 DOCKER_MISSING=1 run_case scripts/enable-mariadb.sh 1
CONFIG_EXISTS=1 NO_ROOT_PASSWORD=1 run_case scripts/enable-mariadb.sh 1
CONFIG_EXISTS=1 NO_CONTAINER_IP=1 run_case scripts/enable-mariadb.sh 1
CONFIG_EXISTS=1 run_case scripts/enable-mariadb.sh 0
check /usr/bin/grep -q "GRANT PROCESS, REPLICATION CLIENT, SELECT" "$FIXTURE_ROOT/sql"
CONFIG_EXISTS=1 MARIADB_CLIENT_MISSING=1 MANAGED_BLOCK='# >>> mariadb (managed by enable-mariadb.sh)' run_case scripts/enable-mariadb.sh 0 test-container
check has_call 'docker exec -i test-container mysql'
check has_call 'sed -i'
CONFIG_EXISTS=1 SERVICE_FAIL=1 run_case scripts/enable-mariadb.sh 1
# General installer: both platform paths use only staging directories.
run_case install.sh 0 --help
run_case install.sh 1 --unknown
run_case install.sh 1 --component invalid
UNIT_UID=501 run_case install.sh 1
MISSING_COMMAND=curl run_case install.sh 1
UNIT_OS=Linux USER_MISSING=1 run_case install.sh 0 --component both --version v0.unit --no-start
check has_call 'monitor-client'
check no_call 'systemctl restart'
UNIT_OS=Linux run_case install.sh 0
check has_call 'curl -fsSL https://api.github.com/repos/codigocentral/code-monitor/releases/latest'
check has_call 'systemctl restart'
UNIT_OS=Darwin UNIT_ARCH=arm64 run_case install.sh 0 --component both
check has_call 'launchctl bootstrap system'
UNIT_OS=Darwin UNIT_ARCH=x86_64 run_case install.sh 0 --component client --no-start
check no_call launchctl
CONFIG_EXISTS=1 UNIT_TOKEN='' run_case scripts/enable-postgres.sh 1
SCAN_CONTENT='token = "Abcd0123456789Abcd1234"' run_case scripts/publish-to-github.sh 1
UNIT_OS=Linux UNIT_ARCH=arm64 run_case install.sh 0
UNIT_OS=Linux UNIT_ARCH=unsupported run_case install.sh 0
UNIT_OS=Darwin UNIT_ARCH=unsupported run_case install.sh 0
UNIT_OS=Unsupported run_case install.sh 1
NO_RELEASE=1 run_case install.sh 0
printf '1..%s\n' "$PLAN"
exit "$((FAILURES>0))"
