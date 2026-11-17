#!/bin/sh
# The image's roles (deploy/docker-compose.yml):
#   key           make the hub's keys (in /hub) if needed, and print its
#                 L2 key: the issuer's, which signs every note
#   hub           the hub, on NETWORK's node (its wallet HUB_WALLET, `hub`
#                 by default), keeping the grants L2's notes in its book
#                 (/hub/book.json): no sequencer
set -eu
NETWORK="${NETWORK:?set NETWORK}"
case "$NETWORK" in
    regtest) chaindir=regtest ;;
    mutinynet | signet) chaindir=signet ;;
    testnet4) chaindir=testnet4 ;;
    *) echo "unknown NETWORK $NETWORK" >&2; exit 1 ;;
esac
cookie="/bitcoind/$chaindir/.cookie"
node=http://bitcoind:8332

# the node writes its cookie when its RPC is up
wait_node() {
    while [ ! -f "$cookie" ]; do echo "waiting for $cookie"; sleep 2; done
}
# a JSON-RPC call to the node: rpc <path> <method> <params>
rpc() {
    curl -sS --user "$(cat "$cookie")" -H 'content-type: text/plain' \
        --data-binary "{\"jsonrpc\":\"1.0\",\"id\":0,\"method\":\"$2\",\"params\":$3}" "$node$1"
}

case "${1:-}" in
    key)
        exec glichen-hub --dir /hub --print-key
        ;;
    hub)
        wait_node
        wallet="${HUB_WALLET:-hub}"
        rpc / loadwallet "[\"$wallet\"]" >/dev/null
        rpc / createwallet "[\"$wallet\"]" >/dev/null
        exec glichen-hub --network "$NETWORK" --listen 0.0.0.0:18310 \
            --rpc-url "$node" --cookie "$cookie" --chain-wallet "$wallet" --dir /hub \
            --statement "${STATEMENT:-hub-proves-mock}" --reserve-ppm "${RESERVE_PPM:-20000}" --fee-ppm "${FEE_PPM:-10000}" --max-liquidity "${MAX_LIQUIDITY:-10000000}" \
            ${DURATION:+--duration "$DURATION"} ${HOSTILE:+--hostile} ${REFUSE_FOLDS:+--refuse-folds}
        ;;
    *)
        exec "$@"
        ;;
esac
