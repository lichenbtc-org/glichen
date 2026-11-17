# gLichen



A layer 2 for [lichen](https://github.com/lichenbtc-org/lichen) that emphasizes: simplicity, throughput and light client support.

## Introduction

CAVEAT: Do not use with real money. Most importantly, the code is unaudited either by humans or LLMs, almost all written by one LLM (extensively *tested* but not *audited*).

The [design paper](./papers/lichen-grants.pdf) is human authored and is the main place to go to understand how it works.

*Want to try it* - with a wallet, against a hub someone is running (a test network only): see [Using a wallet](#using-a-wallet). Or run your own hub: see [Running a hub](#running-a-hub).

-------

## Using a wallet

A user runs only the wallet: no Bitcoin node, and no coins held in it. It
reads Bitcoin from the network's public Esplora server and talks to a
deployment's hub. From the hub's operator you need three things: its URL,
its key (the wallet pins it at the start: check the key it shows you), and
which statement it runs (`hub-proves`, its real proof, or
`hub-proves-mock` on a test stack).

**Deployments.** None is public at the moment. When there is one, it is
listed here with its network, URL, key, statement and the app version
that goes with it. The hub's API is documented at
<https://thimbly.org/lichen/grants/api/>.

### The app

The wallet as a page in your browser (Mac and Linux), or as an app
(desktop, and Android). Build it from `gui/`:

```sh
git clone https://github.com/lichenbtc-org/glichen
cd glichen/gui && cargo build --release -p glichen-gui
target/release/glichen-gui          # opens your browser at the wallet
```

In Settings, choose the network, give the hub's URL, connect, and check
the hub's key it shows. Home opens a contract; Send and Receive pay and
take payments. The desktop app is `cargo tauri build` in `gui/app/`, the
Android one `gui/app/build-android.sh`: see [gui/README.md](gui/README.md).

### The command line

```sh
git clone https://github.com/lichenbtc-org/glichen
cd glichen && cargo build --release -p glichen-wallet     # target/release/glichen

alias gw='target/release/glichen --network mutinynet --statement hub-proves --wallet alice.json --hub <URL>'

gw init                              # a fresh key; check the hub's key it prints
gw open 100000 --liquidity 100000    # prints the hub's fee and a deposit address to pay
gw balance
gw address                           # yours, to be paid to (tlc1…)
gw receive                           # take payments, for a minute
gw send <address> <amount>           # on the L2, while the payee runs receive
gw withdraw <contract> <amount>      # cooperatively: a claim, and the hub folds (0 closes early)
gw exit <contract>                   # if the hub won't: the claim on Bitcoin
```

The deposit is paid from any Bitcoin wallet or faucet; withdrawals,
refunds and your reserve go to the wallet's own payout address (`gw
payout`), or one you name at `open`. Opening uploads about 240 MB (the
garbled circuits the hub keeps). A payment needs the payee online: it
signs too, and the payment is final for both at once. [Using a
wallet](docs/USE_A_WALLET.md) has every step, and what each costs and
waits for.

## Running a hub

[Running a hub](docs/RUN_A_HUB.md) deploys the Bitcoin node and the hub
with Docker Compose and makes them reachable to users: one deployment per
network, its keys made on first start. To try the stack on your own
machine first, on regtest:

```sh
cp deploy/.env.example deploy/.env    # set NETWORK=regtest, COMPOSE_PROFILES=regtest
deploy/build.sh
deploy/lichen.sh up
deploy/smoke.sh     # a wallet opens a contract, is minted its deposit, and withdraws cooperatively
```

`.env.example` runs the mock statement. A real deployment sets
`STATEMENT=hub-proves`: the hub then proves in a dispute (only there),
with RISC Zero and a Groth16 wrap, which needs x86 Linux with Docker and
about 8 GB.

For the quickest look, the GUI's devstack runs a whole regtest
deployment in one process (a hub on the mock statement, a block every 3
seconds, a faucet):

```sh
cd gui && cargo build --release -p glichen-gui     # with bitcoind on the PATH
target/release/glichen-devstack --dir /tmp/devstack
target/release/glichen-gui --devstack /tmp/devstack/devstack.json
```

## The code

On top of lichen's crates (the contract, BABE and the dispute, the hub
and the wallet), pinned at a commit in `Cargo.toml`:

- `crates/notes`: the ledger: notes, payments (spends and their grants), addresses;
- `crates/statement`: the hub's statement, one implementation for the hub, the wallet and the guest;
- `crates/proto`: glichen's part of a contract: the statement's terms and the keys for the dispute's front;
- `crates/hub`, `crates/wallet`: the hub (`glichen-hub`) and the wallet (`glichen`);
- `r0/`: the RISC Zero guest and the hub's prover;
- `gui/`: the wallet's page, the devstack and the app;
- `deploy/`: the Docker deployment ([deploy/README.md](deploy/README.md)).

[docs/CODE.md](docs/CODE.md) has the detail. Tests run on regtest
(`cargo test`, with `bitcoind` on the PATH).

Releases are tagged `grants-vX.Y.Z`, `X.Y` the design's version (the
paper's); the app has the same number. A hub and its apps upgrade
together: the app refuses a hub whose guest isn't its own build's.
