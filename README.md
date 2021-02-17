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
- `Free` : `Allocatable - max (Limit, Requested)`

## Install

### via binary

Download from [github's release](https://github.com/davidB/kubectl-view-allocations/releases/latest) or use script

```sh
curl https://raw.githubusercontent.com/davidB/kubectl-view-allocations/master/scripts/getLatest.sh | bash
```

### via krew (kubectl plugin manager)

[Krew – kubectl plugin manager](https://krew.sigs.k8s.io/)

```sh
kubectl krew install view-allocations
```

### via cargo

```sh
cargo install kubectl-view-allocations
```

## Usage

### Show help

```txt
kubectl-view-allocations -h

kubectl-view-allocations 0.11.0
https://github.com/davidB/kubectl-view-allocations
kubectl plugin to list allocations (cpu, memory, gpu,... X requested, limit, allocatable,...)

USAGE:
    kubectl-view-allocations [FLAGS] [OPTIONS]

FLAGS:
    -h, --help         Prints help information
    -z, --show-zero    Show lines with zero requested and zero limit and zero allocatable
    -V, --version      Prints version information

OPTIONS:
        --context <context>                   The name of the kubeconfig context to use
    -g, --group-by <group-by>...              Group information hierarchically (default: -g resource -g node -g pod)
                                              [possible values: resource, node, pod,
                                              namespace]
    -n, --namespace <namespace>               Show only pods from this namespace
    -o, --output <output>                     Output format [default: table]  [possible values: table,
                                              csv]
    -r, --resource-name <resource-name>...    Filter resources shown by name(s), by default all resources are listed
```

### show gpu allocation

```txt

> kubectl-view-allocations -r gpu

 Resource                   Requested       Limit  Allocatable  Free 
  nvidia.com/gpu           (71%) 10.0  (71%) 10.0         14.0   4.0 
  ├─ node-gpu1               (0%) 0.0    (0%) 0.0          2.0   2.0 
  ├─ node-gpu2               (0%) 0.0    (0%) 0.0          2.0   2.0 
  ├─ node-gpu3             (100%) 2.0  (100%) 2.0          2.0   0.0 
  │  └─ fah-gpu-cpu-d29sc         2.0         2.0                    
  ├─ node-gpu4             (100%) 2.0  (100%) 2.0          2.0   0.0 
  │  └─ fah-gpu-cpu-hkg59         2.0         2.0                    
  ├─ node-gpu5             (100%) 2.0  (100%) 2.0          2.0   0.0 
  │  └─ fah-gpu-cpu-nw9fc         2.0         2.0                    
  ├─ node-gpu6             (100%) 2.0  (100%) 2.0          2.0   0.0 
  │  └─ fah-gpu-cpu-gtwsf         2.0         2.0                    
  └─ node-gpu7             (100%) 2.0  (100%) 2.0          2.0   0.0 
     └─ fah-gpu-cpu-x7zfb         2.0         2.0    
```

### overview only

```sh
> kubectl-view-allocations -g resource

 Resource              Requested          Limit  Allocatable     Free 
  cpu                 (21%) 56.7    (65%) 176.1        272.0     95.9 
  ephemeral-storage     (0%) 0.0       (0%) 0.0        38.4T    38.4T 
  memory             (8%) 52.7Gi  (15%) 101.3Gi      675.6Gi  574.3Gi 
  nvidia.com/gpu      (71%) 10.0     (71%) 10.0         14.0      4.0 
  pods                (9%) 147.0     (9%) 147.0         1.6k     1.5k 
```

### group by namespaces

```sh
> kubectl-view-allocations -g namespace

 Resource              Requested          Limit  Allocatable     Free 
  cpu                 (21%) 56.7    (65%) 176.1        272.0     95.9 
  ├─ default                42.1           57.4                       
  ├─ dev                     5.3          102.1                       
  ├─ dns-external         200.0m            0.0                       
  ├─ docs                 150.0m         600.0m                       
  ├─ ingress-nginx        200.0m            1.0                       
  ├─ kube-system             2.1            1.4                       
  ├─ loki                    1.2            2.4                       
  ├─ monitoring              3.5            7.0                       
  ├─ sharelatex           700.0m            2.4                       
  └─ weave                   1.3            1.8                       
  ephemeral-storage     (0%) 0.0       (0%) 0.0        38.4T    38.4T 
  memory             (8%) 52.7Gi  (15%) 101.3Gi      675.6Gi  574.3Gi 
  ├─ default              34.6Gi         60.0Gi                       
  ├─ dev                   5.3Gi         22.1Gi                       
  ├─ dns-external        140.0Mi        340.0Mi                       
  ├─ docs                448.0Mi        768.0Mi                       
  ├─ ingress-nginx       256.0Mi          1.0Gi                       
  ├─ kube-system         840.0Mi          1.0Gi                       
  ├─ loki                  1.5Gi          1.6Gi                       
  ├─ monitoring            5.9Gi          5.7Gi                       
  ├─ sharelatex            2.5Gi          7.0Gi                       
  └─ weave                 1.3Gi          1.8Gi                       
  nvidia.com/gpu      (71%) 10.0     (71%) 10.0         14.0      4.0 
  └─ dev                    10.0           10.0                       
  pods                (9%) 147.0     (9%) 147.0         1.6k     1.5k 
  ├─ cert-manager            3.0            3.0                       
  ├─ default                13.0           13.0                       
  ├─ dev                     9.0            9.0                       
  ├─ dns-external            2.0            2.0                       
  ├─ docs                    8.0            8.0                       
  ├─ ingress-nginx           2.0            2.0                       
  ├─ kube-system            43.0           43.0                       
  ├─ loki                   12.0           12.0                       
  ├─ monitoring             38.0           38.0                       
  ├─ sharelatex              3.0            3.0                       
  └─ weave                  14.0           14.0   
```

### show as csv

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

## Alternatives

- see the discussion [Need simple kubectl command to see cluster resource usage · Issue #17512 · kubernetes/kubernetes](https://github.com/kubernetes/kubernetes/issues/17512)
