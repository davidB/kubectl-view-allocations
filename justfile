default:
  just --list

k8s_create:
  # k3d cluster create "$CLUSTER_NAME" --agents 2
  sudo systemctl start docker
  kind create cluster --name "$CLUSTER_NAME"
  kubectl cluster-info --context kind-"$CLUSTER_NAME"
  kubectl apply -f tests/metrics-server-components.yaml
  sleep 5
  kubectl top node

k8s_delete:
  # k3d cluster delete "$CLUSTER_NAME"
  kind delete cluster --name "$CLUSTER_NAME"