#!/usr/bin/env bash
while true; do
  date -u +"%Y-%m-%dT%H:%M:%SZ" >> "$HOME/.bridge-logs/keepalive.log"
  curl -s -o /dev/null -m 10 http://127.0.0.1:3300/ || true
  sleep 60
done
