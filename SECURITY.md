# Security Policy

## Supported versions

Code Monitor is pre-1.0. Security fixes land on `main` and in the next tagged
release; older releases are not patched.

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Use GitHub's private vulnerability reporting instead: open the repository's
**Security** tab and choose **Report a vulnerability**. Include:

- the affected version or commit (`monitor-server --version` prints it),
- the platform and how the agent was installed,
- steps to reproduce, and what an attacker gains.

We aim to acknowledge reports within a few business days and will keep you
updated until a fix is released. We are happy to credit reporters in the
release notes unless you prefer to stay anonymous.

## Deployment notes

- The agent exposes host metrics. Bind it to a private or VPN interface, not
  `0.0.0.0` on a public network; `scripts/install-remote.sh` refuses
  `0.0.0.0` for this reason.
- Keep authentication enabled and treat the access token like a password.
  The client stores tokens in its config file with `0600` permissions on Unix.
- Use TLS (or mTLS) whenever traffic leaves a trusted network.
- The systemd units grant the agent no `CAP_NET_ADMIN`; firewall rules are read
  from a snapshot written by a separate root oneshot unit.
