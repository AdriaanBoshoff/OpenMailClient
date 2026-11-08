# Web access

**Status:** revision 1 · 2026-11-08 · web access served by OpenMailClient itself (M7.1) and headless on a server with no display (M7.5), as built and tried on this repository's own machines; reaching it from outside your home network is M7.8 and not written yet

Web access shows your mailbox in a browser on another device: a work PC, a laptop, a phone. It
is served over HTTPS by OpenMailClient itself, from the machine your mail is on; no one else's
server stands in between. It is off until you start it, and it lets in only those who know its
one password. The plan it follows is in [ROADMAP.md](../ROADMAP.md#m7-plan); what is not built
yet is in [docs/gaps.md](gaps.md).

## The password

Web access has one password, set at the machine that serves it, never from a browser:

```text
openmailclient-cli web password            asked for twice; set or reset it
openmailclient-cli web status              is a password set, is it serving, the certificate
openmailclient-cli web bans                addresses banned for failed sign-ins
openmailclient-cli web unban <address>
openmailclient-cli web never-ban <address> an address of yours that is never banned
openmailclient-cli web log                 the last sign-ins and failures
```

Forgot it? Sign in to the machine (at its keyboard or over SSH) and run `web password` again:
every browser that was signed in is signed out. Only an Argon2id hash of the password is kept,
in `web-password.json` beside the settings. Five failed sign-ins from one address within 15
minutes ban it for an hour, and longer each time after.

## Serving from a console

```text
openmailclient serve [--listen 0.0.0.0:8443] [--name mail.home.arpa] [--passwords system|sealed|either]
openmailclient-cli web serve --profile [the same options]
```

Both serve your own mailbox, the one the window opens: the same settings, data files and
accounts, received from and sent through as the window does. The second needs no display at
all. It prints the addresses it is reached at and the SHA-256 fingerprint of its certificate:
the browser warns that it does not know the certificate the first time, and you go on only when
the fingerprint it shows is the one printed. `--listen 127.0.0.1:8443`, the default, lets in
this machine alone; `0.0.0.0:8443` lets in every network the machine is on.

Only one program uses a mailbox at a time: while `serve` runs, the window will not open the
same settings and says who has them, and the other way round. Stop it with Ctrl+C, or with
the service manager's stop: what is being saved is finished first (up to two minutes), and
anything not yet saved is in the journal, put back on the next start.

## Mail account passwords on a server

The window keeps each account's password in the system's credential store (the Windows
Credential Manager, the macOS Keychain, the Secret Service on a Linux desktop). A server has
no desktop and often no Secret Service, so there they are sealed instead: encrypted with
ChaCha20-Poly1305 in `passwords.sealed` beside the settings, under a key kept apart from them,
in `~/.local/share/openmailclient-keys/passwords.key` (readable by you alone), or given by
systemd as the credential `openmailclient-passwords-key`. A copy of the settings' folder
carries no password and no key. `--passwords` chooses: `system`, `sealed`, or `either`, the
default, which uses the system's store when it answers and the sealed file otherwise.

Set an account's password over SSH, asked for twice and never on the command line:

```text
openmailclient-cli account list
openmailclient-cli account password you@example.org
openmailclient-cli account password you@example.org --forget
```

A mail account's password is never sent to a browser.

## As a service on Ubuntu Server or Debian

`install.sh` from the Linux archive puts the programs under `~/.local` and a systemd user
service in `~/.config/systemd/user/openmailclient.service`, which it does not start. Then, as
the user whose mail it is:

```sh
openmailclient-cli web password
openmailclient-cli account password you@example.org     # for each account, if the window never ran here
systemctl --user enable --now openmailclient
loginctl enable-linger "$USER"                           # keep it running when you are not signed in
journalctl --user -u openmailclient                      # the addresses and the fingerprint
```

The service serves on every network the machine is on (`--listen 0.0.0.0:8443`); edit it with
`systemctl --user edit --full openmailclient` to change that, or to add `--name` with the name
the machine is known by. Accounts are added in the window on a desktop, or with
`openmailclient-cli run --profile --cmd 'account.add={…}'` (see `openmailclient-cli commands`),
while the service is stopped.

For a system service run as a user of its own, the key can be kept encrypted by systemd
(`systemd-creds encrypt`) rather than in a file of that user's:

```ini
[Service]
User=mail-owner
LoadCredentialEncrypted=openmailclient-passwords-key:/etc/credstore.encrypted/openmailclient-passwords-key
ExecStart=/home/mail-owner/.local/share/openmailclient/openmailclient-cli web serve --profile --listen 0.0.0.0:8443 --passwords sealed
```

The key is 32 random bytes (or the same in base64): make one with `head -c 32 /dev/urandom`
and give it to `systemd-creds encrypt` before the first password is set. A key given by systemd
is only read: OpenMailClient never makes one there, and never writes over passwords sealed
with another key.

`openmailclient-cli web status` says whether it is serving. Sign-ins and failures go to
`web-sign-ins.log` beside the settings.

## Revision history

| Revision | Date | Change |
|---|---|---|
| 1 | 2026-11-08 | the console, the password, sealed account passwords and the systemd service (M7.1, M7.5) |
