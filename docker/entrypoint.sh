#!/bin/sh
# The image's entry point: `energyml` with its arguments, or a program given
# instead.
#
# `docker run <image> check model.epc` runs `energyml check model.epc`. But a
# workflow engine runs a tool's command line through a shell - Galaxy calls
# `docker run <image> /bin/sh -c "<command>"` - and with `energyml` as the entry
# point that would become `energyml /bin/sh -c …`, refused as an unknown
# subcommand. So a first argument that is a path, or one of the programs below,
# is run as it is.
set -e
case "${1:-}" in
  "") exec energyml --help ;;
  /*|./*) exec "$@" ;;
  sh|bash|python3|python|env|enyml|energyml-mcp) exec "$@" ;;
  *) exec energyml "$@" ;;
esac
