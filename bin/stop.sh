#!/usr/bin/env bash
BASE="$( cd "$( dirname "${BASH_SOURCE[0]}" )/.." && pwd )"
echo $BASE
cd $BASE

ps aux|grep llm-audit|grep -v grep|awk '{print $2}'|xargs kill -9
echo "llm-audit stopped"