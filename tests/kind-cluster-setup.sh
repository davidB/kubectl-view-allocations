#!/bin/bash

sudo systemctl start docker
kind create cluster
kubectl cluster-info --context kind-kind
kubectl apply -f ./metrics-server-components.yaml
sleep 5
kubectl top node

echo "kind delete cluster"
