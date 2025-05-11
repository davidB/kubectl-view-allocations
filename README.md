# kubectl-view-allocations

[![Crates.io](https://img.shields.io/crates/l/kubectl-view-allocations.svg)](http://creativecommons.org/publicdomain/zero/1.0/)
[![Crates.io](https://img.shields.io/crates/v/kubectl-view-allocations.svg)](https://crates.io/crates/kubectl-view-allocations)

[![Project Status: WIP – Initial development is in progress, but there has not yet been a stable, usable release suitable for the public.](https://www.repostatus.org/badges/latest/wip.svg)](https://www.repostatus.org/#wip)
[![Actions Status](https://github.com/davidB/kubectl-view-allocations/workflows/ci-flow/badge.svg)](https://github.com/davidB/kubectl-view-allocations/actions)
[![Documentation](https://docs.rs/kubectl-view-allocations/badge.svg)](https://docs.rs/kubectl-view-allocations/)

[![Crates.io](https://img.shields.io/crates/d/kubectl-view-allocations.svg)](https://crates.io/crates/kubectl-view-allocations)
![GitHub All Releases](https://img.shields.io/github/downloads/davidB/kubectl-view-allocations/total.svg)

`kubectl` plugin lists allocations for resources (cpu, memory, gpu,...) as defined into the manifest of nodes and running pods. It doesn't list usage like `kubectl top`. It can provide result grouped by namespaces, nodes, pods and filtered by resources'name.

Columns displayed :

- `Requested` : Quantity of resources requested by the container in the pod's manifest. It's the sum group by pod, namespace, node where container is running. With percentage of resources requested over what is allocatable in the group.
- `Limit` : Quantity of resources max (limit) requestable by the container in the pod's manifest. It's the sum group by pod, namespace, node where container is running. With percentage of resources max / limit over what is allocatable in the group.
- `Allocatable` : Allocatable resources defined (or detected) on nodes.
- `Free` : `Allocatable - max (Limit, Requested)` (by default, see options `--used-mode`)
- `Utilization` : Quantity of resources (cpu & memory only) used as reported by Metrics API. It's disable by default, [metrics-server](https://github.com/kubernetes-incubator/metrics-server) is optional and should be setup into the cluster.

## Install

### Via download binary

Download from [github's release](https://github.com/davidB/kubectl-view-allocations/releases/latest) or use script

```sh
curl https://raw.githubusercontent.com/davidB/kubectl-view-allocations/master/scripts/getLatest.sh | bash
```

### Via krew (kubectl plugin manager)

[Krew – kubectl plugin manager](https://krew.sigs.k8s.io/)

```sh
kubectl krew install view-allocations
```

### Via cargo

```sh
cargo install kubectl-view-allocations
```

### As lib in Cargo.toml

If you want to embed some function or struct of the plugin into an other rust code:

```toml
[dependencies]
kubectl-view-allocations = { version = "0.14", default-features = false }

[features]
default = ["k8s-openapi/v1_20"]
```

## Usage

### Show help

```sh
> kubectl-view-allocations -h
kubectl plugin to list allocations (cpu, memory, gpu,...) X (utilization, requested, limit, allocatable,...)

Usage: kubectl-view-allocations [OPTIONS]

Options:
      --context <CONTEXT>
          The name of the kubeconfig context to use
  -n, --namespace <NAMESPACE>
          Show only pods from this namespace
  -l, --selector <SELECTOR>
          Show only nodes match this label selector
  -u, --utilization
          Force to retrieve utilization (for cpu and memory), requires having metrics-server https://github.com/kubernetes-sigs/metrics-server
  -z, --show-zero
          Show lines with zero requested AND zero limit AND zero allocatable, OR pods with unset requested AND limit for `cpu` and `memory`
      --used-mode <USED_MODE>
          The way to compute the `used` part for free (`allocatable - used`) [default: max-request-limit] [possible values: max-request-limit, only-request]
      --precheck
          Pre-check access and refresh token on kubeconfig by running `kubectl cluster-info`
      --accept-invalid-certs
          Accept invalid certificates (dangerous)
  -r, --resource-name <RESOURCE_NAME>...
          Filter resources shown by name(s), by default all resources are listed
  -g, --group-by <GROUP_BY>...
          Group information hierarchically (default: `-g resource -g node -g pod`) [possible values: resource, node, pod, namespace]
  -o, --output <OUTPUT>
          Output format [default: table] [possible values: table, csv]
  -h, --help
          Print help
  -V, --version
          Print version

https://github.com/davidB/kubectl-view-allocations
```

### Show gpu allocation

```sh

> kubectl-view-allocations -r gpu

 Resource                   Requested       Limit  Allocatable  Free
  nvidia.com/gpu           (71%) 10.0  (71%) 10.0         14.0   4.0
  ├─ node-gpu1               (0%)  __    (0%)  __          2.0   2.0
  ├─ node-gpu2               (0%)  __    (0%)  __          2.0   2.0
  ├─ node-gpu3             (100%) 2.0  (100%) 2.0          2.0    __
  │  └─ fah-gpu-cpu-d29sc         2.0         2.0           __    __
  ├─ node-gpu4             (100%) 2.0  (100%) 2.0          2.0    __
  │  └─ fah-gpu-cpu-hkg59         2.0         2.0           __    __
  ├─ node-gpu5             (100%) 2.0  (100%) 2.0          2.0    __
  │  └─ fah-gpu-cpu-nw9fc         2.0         2.0           __    __
  ├─ node-gpu6             (100%) 2.0  (100%) 2.0          2.0    __
  │  └─ fah-gpu-cpu-gtwsf         2.0         2.0           __    __
  └─ node-gpu7             (100%) 2.0  (100%) 2.0          2.0    __
     └─ fah-gpu-cpu-x7zfb         2.0         2.0           __    __
```

### Overview only

```sh
> kubectl-view-allocations -g resource

 Resource              Requested          Limit  Allocatable     Free
  cpu                 (21%) 56.7    (65%) 176.1        272.0     95.9
  ephemeral-storage     (0%)  __       (0%)  __        38.4T    38.4T
  memory             (8%) 52.7Gi  (15%) 101.3Gi      675.6Gi  574.3Gi
  nvidia.com/gpu      (71%) 10.0     (71%) 10.0         14.0      4.0
  pods                (9%) 147.0     (9%) 147.0         1.6k     1.5k
```

### Show utilization

- Utilization information are retrieve from [metrics-server](https://github.com/kubernetes-incubator/metrics-server) (should be setup on your cluster).
- Only report cpu and memory utilization

```sh
> kubectl-view-allocations -u

 Resource                                        Utilization     Requested         Limit  Allocatable   Free
  cpu                                              (0%) 9.0m  (10%) 200.0m            __          2.0    1.8
  └─ lima-rancher-desktop                          (0%) 9.0m  (10%) 200.0m            __          2.0    1.8
     ├─ coredns-96cc4f57d-57cj9                         1.0m        100.0m            __           __     __
     ├─ local-path-provisioner-84bb864455-czzcg         1.0m            __            __           __     __
     ├─ metrics-server-ff9dbcb6c-kb7x9                  4.0m        100.0m            __           __     __
     ├─ svclb-traefik-ggd2q                             2.0m            __            __           __     __
     └─ traefik-55fdc6d984-sqp57                        1.0m            __            __           __     __
  ephemeral-storage                                       __            __            __        99.8G     __
  └─ lima-rancher-desktop                                 __            __            __        99.8G     __
  memory                                         (1%) 51.0Mi  (2%) 140.0Mi  (3%) 170.0Mi        5.8Gi  5.6Gi
  └─ lima-rancher-desktop                        (1%) 51.0Mi  (2%) 140.0Mi  (3%) 170.0Mi        5.8Gi  5.6Gi
     ├─ coredns-96cc4f57d-57cj9                       11.5Mi        70.0Mi       170.0Mi           __     __
     ├─ local-path-provisioner-84bb864455-czzcg        6.2Mi            __            __           __     __
     ├─ metrics-server-ff9dbcb6c-kb7x9                14.9Mi        70.0Mi            __           __     __
     ├─ svclb-traefik-ggd2q                          548.0Ki            __            __           __     __
     └─ traefik-55fdc6d984-sqp57                      17.9Mi            __            __           __     __
  pods                                                    __      (5%) 5.0      (5%) 5.0        110.0  105.0
  └─ lima-rancher-desktop                                 __      (5%) 5.0      (5%) 5.0        110.0  105.0
```

### Group by namespaces

```sh
> kubectl-view-allocations -g namespace

 Resource               Requested         Limit  Allocatable   Free
  cpu                (10%) 200.0m            __          2.0    1.8
  └─ kube-system           200.0m            __           __     __
  ephemeral-storage            __            __        99.8G     __
  memory             (2%) 140.0Mi  (3%) 170.0Mi        5.8Gi  5.6Gi
  └─ kube-system          140.0Mi       170.0Mi           __     __
  pods                   (5%) 5.0      (5%) 5.0        110.0  105.0
  └─ kube-system              5.0           5.0           __     __
```

### Show as csv

In this case value as expanded as float (with 2 decimal)

```sh
kubectl-view-allocations -o csv
Date,Kind,resource,node,pod,Requested,%Requested,Limit,%Limit,Allocatable,Free
2020-08-19T19:12:48.326605746+00:00,resource,cpu,,,59.94,22%,106.10,39%,272.00,165.90
2020-08-19T19:12:48.326605746+00:00,node,cpu,node-gpu1,,2.31,19%,4.47,37%,12.00,7.53
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu1,yyy-b8bd56fbd-5x8vq,1.00,,2.00,,,
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu1,kube-flannel-ds-amd64-7dz9z,0.10,,0.10,,,
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu1,node-exporter-gpu-b4w7s,0.11,,0.22,,,
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu1,xxx-backend-7d84544458-46qnh,1.00,,2.00,,,
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu1,weave-scope-agent-bbdnz,0.10,,0.15,,,
2020-08-19T19:12:48.326605746+00:00,node,cpu,node-gpu2,,0.31,1%,0.47,2%,24.00,23.53
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu2,kube-flannel-ds-amd64-b5b4v,0.10,,0.10,,,
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu2,node-exporter-gpu-796jz,0.11,,0.22,,,
2020-08-19T19:12:48.326605746+00:00,pod,cpu,node-gpu2,weave-scope-agent-8rhnd,0.10,,0.15,,,
2020-08-19T19:12:48.326605746+00:00,node,cpu,node-gpu3,,3.41,11%,6.67,21%,32.00,25.33
...
```

It can be combined with "group-by" options.

```sh
kubectl-view-allocations -g resource -o csv
Date,Kind,resource,Requested,%Requested,Limit,%Limit,Allocatable,Free
2020-08-19T19:11:49.630864028+00:00,resource,cpu,59.94,22%,106.10,39%,272.00,165.90
2020-08-19T19:11:49.630864028+00:00,resource,ephemeral-storage,0.00,0%,0.00,0%,34462898618662.00,34462898618662.00
2020-08-19T19:11:49.630864028+00:00,resource,hugepages-1Gi,0.00,,0.00,,,
2020-08-19T19:11:49.630864028+00:00,resource,hugepages-2Mi,0.00,,0.00,,,
2020-08-19T19:11:49.630864028+00:00,resource,memory,69063409664.00,10%,224684670976.00,31%,722318667776.00,497633996800.00
2020-08-19T19:11:49.630864028+00:00,resource,nvidia.com/gpu,3.00,27%,3.00,27%,11.00,8.00
2020-08-19T19:11:49.630864028+00:00,resource,pods,0.00,0%,0.00,0%,1540.00,1540.00
```

## Alternatives & Similars

- see the discussion [Need simple kubectl command to see cluster resource usage · Issue #17512 · kubernetes/kubernetes](https://github.com/kubernetes/kubernetes/issues/17512)
- For CPU & Memory only
  - [robscott/kube-capacity: A simple CLI that provides an overview of the resource requests, limits, and utilization in a Kubernetes cluster](https://github.com/robscott/kube-capacity),
  - [hjacobs/kube-resource-report: Report Kubernetes cluster and pod resource requests vs usage and generate static HTML](https://github.com/hjacobs/kube-resource-report)
  - [etopeter/kubectl-view-utilization: kubectl plugin to show cluster CPU and Memory requests utilization](https://github.com/etopeter/kubectl-view-utilization)
- For CPU & Memory utilization only
  - `kubectl top pods`
  - [LeastAuthority/kubetop: A top(1)-like tool for Kubernetes.](https://github.com/LeastAuthority/kubetop)
