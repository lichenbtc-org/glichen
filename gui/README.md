# lichen-gui

A lichen wallet as a page in your browser, for Mac and Linux laptops, and
as an app (Android, and the desktop): see "The app" below.
`lichen-gui` serves the page on 127.0.0.1 only and opens your browser at
it. The URL carries a token made fresh at each launch, and every call to
the wallet must carry it, so other pages in your browser can't drive the
wallet.

Four tabs: **Home** (your balance, your contracts and opening one, and
your activity), **Send**, **Receive** (your L2 address, with a QR code)
and **Settings** (the network and hub, its terms, your Bitcoin, backup).

A local project: it builds against lichen's crates by path and
isn't part of lichen.

## Build

You need git and Rust ([rustup](https://rustup.rs); `rust-toolchain.toml`
picks the version). Then:

```sh
git clone -b grants https://github.com/lichenbtc-org/lichen-gui
cd lichen-gui
./build.sh                 # target/release/lichen-gui and lichen-devstack
```

It builds against lichen's crates by path, so `build.sh` clones
[lichen](https://github.com/lichenbtc-org/lichen) beside this directory
(`../lichen`) at the tag in `LICHEN_TAG` (lichen-btc comes by git, as
lichen pins it). A checkout already there is left as it is: at
another revision, or with changes to its files, the script stops and
says what to do (`LICHEN_GUI_DEV=1` builds against it anyway).

| lichen-gui | lichen |
|---|---|
| v0.1.0 | v0.4.1 |
| v0.1.1 | v0.4.2 |
| v0.2.0 | v0.4.3 |
| v0.2.1 | v0.4.5 |
| v0.2.2 | v0.4.5 |

**The grants app** ("lichen grants", a separate app beside the anchored
L2's, branch `grants` of both repos) has one number with the lichen it
builds on: release `X.Y.Z`, tagged `grants-vX.Y.Z` in both, X.Y the
design's version (papers/lichen-grants.pdf). Settings shows the app's
and the hub's. (Before this numbering, grants builds were app v0.5.0 and
v0.6.0, untagged.)

| lichen grants app | lichen |
|---|---|
| 0.5.1 | grants-v0.5.1 (design v0.5: one note per account) |
| 0.5.2 | grants-v0.5.2 (byte strings as hex on the wire: a hub and its apps upgrade together) |
| 0.5.3 | grants-v0.5.3 (addresses as `tlc1…`, bech32m with the hub's tag; the phone's camera scans one; payments received shown in Activity) |

A hub tells wallets which guests it proves with, and the GUI refuses one
whose guests aren't its own build's: use the lichen version the hub runs
(its terms panel shows it).

## Use

```sh
target/release/lichen-gui
```

In Settings, choose the network, give the hub's URL and connect. The
first time, the GUI shows the hub's key: check it against the one the
hub's operator publishes, then confirm. It makes a wallet pinned to that
hub. The chain is read from the network's public Esplora
(mutinynet.com's, or mempool.space's for signet); under "chain source" you
can name others, or a node.

At launch the GUI reconnects to the last hub, and says so under the
connection. The header shows "not updating" if refreshes stop; restart the
GUI if it stays.

The wallet has a Bitcoin address of its own ("Your Bitcoin"), the default
payout address of a contract: withdrawals, refunds and reserves arrive
there, and you can send from it, or pay the next contract's deposit from
it when an opening asks. So beyond a faucet for the first deposit, no
other wallet is needed. A contract with nothing left to withdraw can be
closed early (your reserve back), and one past its end whose default the
hub hasn't posted, you can post yourself.

Its data is in `~/Library/Application Support/lichen-gui` (Mac) or
`~/.local/share/lichen-gui` (Linux), or `--dir`. There's one wallet file per
network and hub key in `wallets/`; each holds secrets, so keep the
directory private and back it up.

## The app (Android, and the desktop)

The same page and wallet as an app, with Tauri (`app/`): the page in a
webview, the wallet's API a command rather than HTTP. Its wallets and
settings live in the app's own data directory.

- **Android** (arm64, sideloaded): `app/build-android.sh` builds
  `target/lichen-grants-<version>-arm64.apk`. It needs JDK 17, the Android SDK
  (platform 35+, build-tools, an NDK), the Tauri CLI
  (`cargo install tauri-cli`) and Rust's `aarch64-linux-android` target.
  Install with `adb install -r`, or copy the file to the phone and open
  it (allow installs from that app).
- **Desktop:** `cargo tauri build --bundles app` in `app/` (the Mac's
  `target/release/bundle/macos/lichen grants.app`), or `cargo tauri dev`.

**The signing key** is `~/.android/debug.keystore` on the Mac that built
the first APK (made by `build-android.sh`, password `android`): an update
installs over the app, keeping its wallet, only if signed with the same
key. Back it up. Signed with another, an update needs the app
uninstalled first, which deletes its wallet (unless backed up, below).

**The networks.** Settings offers the test deployment on lichen-hub,
behind TLS at `https://thimbly.org/lichen/grants/hub` (Caddy on
thimbly.org's relay): Mutinynet, whose key the app knows (its wallet is
made without asking you to check it, and refused if it differs). Or
another hub by its URL, whose key you check.

**Opening a contract** shows its steps: pay the deposit (its QR code and
address; seen, confirmed), garbling (78 instances: 40 s on a 4-core
emulator), the upload of the 10 the hub keeps (239 MB; 3.5 MB/s from the
emulator through the relay), the hub's check, the funding, open. About
two minutes on Mutinynet once the deposit confirms. Keep the app in front
until it's done; if it's stopped, "Resume the opening" on Home finishes
it from the deposit.

**Addresses** (the grants L2) look like `tlc1q…` (70 characters):
bech32m, with a checksum and a tag of the hub, so a typo or an address
of another hub is refused before anything is sent. The QR code on Receive
carries it upper case (a smaller code); either case is read. On the
phone, Send has **Scan a QR code**: the camera, asked for at the first
scan.

**Payments** (the grants L2, one note per account): both sides sign, so
the receiver's app must be open; it takes and signs a payment to it by
itself, and the payment is final for both at once, in a few seconds
(3.2 s on the devstack). A payment the receiver doesn't take within a
minute is cancelled; one in flight can be cancelled from Send.

**Backup** (Settings): the wallet's file, through the system's file
picker; restored as the wallet for its deployment if the device has none
(or has the same one). It holds the wallet's key.

## Developing: the devstack

`lichen-devstack` runs a whole deployment locally in one process: a
regtest bitcoind (`bitcoind` on the PATH, or `LICHEN_BITCOIND`), a
hub on BABE's mock statement (no proving), a block every 3
seconds, and a faucet. Each run starts fresh, as a new deployment.

```sh
target/release/lichen-devstack --dir /tmp/devstack       # Ctrl-C stops it
target/release/lichen-gui --devstack /tmp/devstack/devstack.json
```

Opening a contract then offers "Pay it from the devstack's faucet".
`--refuse-folds` makes the hub refuse withdrawals, to see what the GUI
does then.
