#!/usr/bin/env bash
#
# Start a full node
#
here=$(dirname "$0")
# shellcheck source=multinode-demo/common.sh
source "$here"/common.sh

# shellcheck source=scripts/oom-score-adj.sh
source "$here"/../scripts/oom-score-adj.sh

if [[ -d "$SNAP" ]]; then
  # Exit if mode is not yet configured
  # (typically the case after the Snap is first installed)
  [[ -n "$(snapctl get mode)" ]] || exit 0
fi

usage() {
  if [[ -n $1 ]]; then
    echo "$*"
    echo
  fi
  cat <<EOF
usage: $0 [-x] [--blockstream PATH] [--init-complete-file FILE] [--no-leader-rotation] [--no-signer] [--rpc-port port] [rsync network path to bootstrap leader configuration] [network entry point]

Start a full node on the specified network

  -x                    - start a new, dynamically-configured full node
  -X [label]            - start or restart a dynamically-configured full node with
                          the specified label
  --blockstream PATH    - open blockstream at this unix domain socket location
  --init-complete-file FILE - create this file, if it doesn't already exist, once node initialization is complete
  --no-leader-rotation  - disable leader rotation
  --no-signer           - start node without vote signer
  --rpc-port port       - custom RPC port for this node

EOF
  exit 1
}

if [[ $1 = -h ]]; then
  usage
fi

maybe_blockstream=
maybe_init_complete_file=
maybe_no_leader_rotation=
maybe_no_signer=
maybe_rpc_port=
self_setup=0

while [[ ${1:0:1} = - ]]; do
  if [[ $1 = -X ]]; then
    self_setup=1
    self_setup_label=$2
    shift 2
  elif [[ $1 = -x ]]; then
    self_setup=1
    self_setup_label=$$
    shift
  elif [[ $1 = --blockstream ]]; then
    maybe_blockstream="$1 $2"
    shift 2
  elif [[ $1 = --init-complete-file ]]; then
    maybe_init_complete_file="--init-complete-file $2"
    shift 2
  elif [[ $1 = --no-leader-rotation ]]; then
    maybe_no_leader_rotation="--no-leader-rotation"
    shift
  elif [[ $1 = --no-signer ]]; then
    maybe_no_signer="--no-signer"
    shift
  elif [[ $1 = --rpc-port ]]; then
    maybe_rpc_port="$1 $2"
    shift 2
  else
    echo "Unknown argument: $1"
    exit 1
  fi
done

if [[ -d $SNAP ]]; then
  if [[ $(snapctl get leader-rotation) = false ]]; then
    maybe_no_leader_rotation="--no-leader-rotation"
  fi
fi

if [[ -n $3 ]]; then
  usage
fi

find_leader() {
  declare leader leader_address
  declare shift=0
  if [[ -d $SNAP ]]; then
    if [[ -n $1 ]]; then
      usage "Error: unexpected parameter: $1"
    fi

    # Select leader from the Snap configuration
    leader_ip=$(snapctl get entrypoint-ip)
    if [[ -z $leader_ip ]]; then
      leader=testnet.solana.com
      leader_ip=$(dig +short "${leader%:*}" | head -n1)
      if [[ -z $leader_ip ]]; then
          usage "Error: unable to resolve IP address for $leader"
      fi
    fi
    leader=$leader_ip
    leader_address=$leader_ip:8001
  else
    if [[ -z $1 ]]; then
      leader=${here}/..        # Default to local tree for rsync
      leader_address=127.0.0.1:8001 # Default to local leader
    elif [[ -z $2 ]]; then
      leader=$1

      declare leader_ip
      if [[ $leader =~ ^[0-9]{1,3}.[0-9]{1,3}.[0-9]{1,3}.[0-9]{1,3}$ ]]; then
        leader_ip=$leader
      else
        leader_ip=$(dig +short "${leader%:*}" | head -n1)

        if [[ -z $leader_ip ]]; then
            usage "Error: unable to resolve IP address for $leader"
        fi
      fi

      leader_address=$leader_ip:8001
      shift=1
    else
      leader=$1
      leader_address=$2
      shift=2
    fi
  fi

  echo "$leader" "$leader_address" "$shift"
}

read -r leader leader_address shift < <(find_leader "${@:1:2}")
shift "$shift"

if [[ -n $SOLANA_CUDA ]]; then
  program=$solana_fullnode_cuda
else
  program=$solana_fullnode
fi

if ((!self_setup)); then
  [[ -f $SOLANA_CONFIG_DIR/fullnode.json ]] || {
    echo "$SOLANA_CONFIG_DIR/fullnode.json not found, create it by running:"
    echo
    echo "  ${here}/setup.sh"
    exit 1
  }
  fullnode_id_path=$SOLANA_CONFIG_DIR/fullnode-id.json
  fullnode_json_path=$SOLANA_CONFIG_DIR/fullnode.json
  ledger_config_dir=$SOLANA_CONFIG_DIR/fullnode-ledger
  accounts_config_dir=$SOLANA_CONFIG_DIR/fullnode-accounts
else
  mkdir -p "$SOLANA_CONFIG_DIR"
  fullnode_id_path=$SOLANA_CONFIG_DIR/fullnode-id-x$self_setup_label.json
  [[ -f "$fullnode_id_path" ]] || $solana_keygen -o "$fullnode_id_path"

  mkdir -p "$SOLANA_CONFIG_DIR"
  fullnode_json_path=$SOLANA_CONFIG_DIR/fullnode-x$self_setup_label.json
  [[ -f "$fullnode_json_path" ]] || {
    echo "Finding a port.."
    # Find an available port in the range 9100-9899
    (( port = 9100 + ($$ % 800) ))
    while true; do
      (( port = port >= 9900 ? 9100 : ++port ))
      echo "Testing $port"
      if ! nc -w 10 -z 127.0.0.1 $port; then
        echo "Selected port $port"
        break;
      fi
      echo "Port $port is in use"
    done
    $solana_fullnode_config --keypair="$fullnode_id_path" -l -b "$port" > "$fullnode_json_path"
  }

  ledger_config_dir=$SOLANA_CONFIG_DIR/fullnode-ledger-x$self_setup_label
  accounts_config_dir=$SOLANA_CONFIG_DIR/fullnode-accounts-x$self_setup_label
fi

[[ -r $fullnode_id_path ]] || {
  echo "$fullnode_id_path does not exist"
  exit 1
}

tune_system

rsync_url() { # adds the 'rsync://` prefix to URLs that need it
  declare url="$1"

  if [[ $url =~ ^.*:.*$ ]]; then
    # assume remote-shell transport when colon is present, use $url unmodified
    echo "$url"
    return 0
  fi

  if [[ -d $url ]]; then
    # assume local directory if $url is a valid directory, use $url unmodified
    echo "$url"
    return 0
  fi

  # Default to rsync:// URL
  echo "rsync://$url"
}

rsync_leader_url=$(rsync_url "$leader")
set -ex
if [[ ! -d "$ledger_config_dir" ]]; then
  $rsync -vPr "$rsync_leader_url"/config/ledger/ "$ledger_config_dir"
  [[ -d $ledger_config_dir ]] || {
    echo "Unable to retrieve ledger from $rsync_leader_url"
    exit 1
  }
  $solana_ledger_tool --ledger "$ledger_config_dir" verify

  $solana_wallet --keypair "$fullnode_id_path" address

  # A fullnode requires 3 tokens to function:
  # - one token to create an instance of the vote_program with
  # - one token for the transaction fee
  # - one token to keep the node identity public key valid.
  retries=5
  while true; do
    # TODO: Until https://github.com/solana-labs/solana/issues/2355 is resolved
    # a fullnode needs N tokens as its vote account gets re-created on every
    # node restart, costing it tokens
    if $solana_wallet --keypair "$fullnode_id_path" --host "${leader_address%:*}" airdrop 1000000; then
      break
    fi

    # TODO: Consider moving this retry logic into `solana-wallet airdrop` itself,
    #       currently it does not retry on "Connection refused" errors.
    retries=$((retries - 1))
    if [[ $retries -le 0 ]]; then
      exit 1
    fi
    echo "Airdrop failed. Remaining retries: $retries"
    sleep 1
  done
fi

trap 'kill "$pid" && wait "$pid"' INT TERM
# shellcheck disable=SC2086 # Don't want to double quote maybe_blockstream or maybe_init_complete_file or maybe_no_signer or maybe_rpc_port
$program \
  $maybe_blockstream \
  $maybe_init_complete_file \
  $maybe_no_leader_rotation \
  $maybe_no_signer \
  $maybe_rpc_port \
  --identity "$fullnode_json_path" \
  --network "$leader_address" \
  --ledger "$ledger_config_dir" \
  --accounts "$accounts_config_dir" \
  > >($fullnode_logger) 2>&1 &
pid=$!
oom_score_adj "$pid" 1000
wait "$pid"
