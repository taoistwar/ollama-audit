#!/usr/bin/env bash
BASE="$( cd "$( dirname "${BASH_SOURCE[0]}" )/.." && pwd )"
echo $BASE
cd $BASE
mkdir -p $BASE/logs
nohup $BASE/bin/ollama-audit > $BASE/logs/ollama-audit.log 2>&1 &
