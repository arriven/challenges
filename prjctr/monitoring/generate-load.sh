#!/usr/bin/env bash

source <(curl https://raw.githubusercontent.com/Arriven/db1000n/main/install.sh)
./db1000n -c loadgen.yaml --refresh-interval=10s --scale=10 --country-list ""