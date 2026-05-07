#!/usr/bin/env bash
BASE="$( cd "$( dirname "${BASH_SOURCE[0]}" )/.." && pwd )"
echo "base:$BASE"
cd $BASE
mkdir -p $BASE/logs
nohup $BASE/bin/llm-audit > $BASE/logs/llm-audit.log 2>&1 &\
echo "logs: $BASE/logs/llm-audit.log"
echo "pid: $!"
echo "llm-audit started"