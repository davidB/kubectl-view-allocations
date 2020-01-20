#!/bin/bash

#set -ex

apt-get update && apt-get install -y curl
#curl https://raw.githubusercontent.com/davidB/kubectl-view-allocations/master/scripts/getLatest.sh | sh
#sh /prj/scripts/getLatest.sh
bash /prj/scripts/getLatest.sh

ls -l /tmp
kubectl-view-allocations
