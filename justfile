default:
  just --list

k8s_create_kind:
  # k3d cluster create "$CLUSTER_NAME" --agents 2
  sudo systemctl start docker
  kind create cluster --name "$CLUSTER_NAME"
  kubectl cluster-info --context kind-"$CLUSTER_NAME"
  kubectl apply -f tests/metrics-server-components.yaml
  sleep 5
  kubectl top node
  cargo run

k8s_delete_kind:
  # k3d cluster delete "$CLUSTER_NAME"
  kind delete cluster --name "$CLUSTER_NAME"

# k8s_create_kwok_in_container:
#   cp tests/kube_config-kwokcontainer.yaml $HOME/.kube/config-kwokcontainer.yaml
#   kubectl config --kubeconfig=config-kwokcontainer use-context kwok
#   podman run --rm -it -p 8080:8080 registry.k8s.io/kwok/cluster:v0.4.0-k8s.v1.28.0

k8s_create_kwok:
  # echo "require docker, with podman I got timeout on my machine"
  kwokctl create cluster --name="$CLUSTER_NAME"
  kwokctl get clusters
  kubectl cluster-info --context kwok-"$CLUSTER_NAME"
  kwokctl scale node --replicas 2 --name="$CLUSTER_NAME"
  kubectl get node
  kubectl create deployment pod --image=pod --replicas=5
  kubectl get pods -o wide
  echo "use '--accept-invalid-certs' with kube view-allocations"
  cargo run -- --accept-invalid-certs

k8s_delete_kwok:
  kwokctl delete cluster --name="$CLUSTER_NAME"
