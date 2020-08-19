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

- `Requested` : Quantity of resources requested by the container in the pod's manifest. It's the sum group by pod, namespace, node where container is running.
- `%Requested` : Percentage of resources requested over what is allocatable in the group.
- `Limit` : Quantity of resources max (limit) requestable by the container in the pod's manifest. It's the sum group by pod, namespace, node where container is running.
- `%Limit` : Percentage of resources max / limit over what is allocatable in the group.
- `Allocatable` : Allocatable resources defined (or detected) on nodes.
- `Free` : `Allocatable - max (Limit, Requested)`

## Install

### via binary

Download from [github's release](https://github.com/davidB/kubectl-view-allocations/releases/latest) or use script

```sh
curl https://raw.githubusercontent.com/davidB/kubectl-view-allocations/master/scripts/getLatest.sh | bash
```

### via cargo

```sh
cargo install kubectl-view-allocations
```

## Usage

### Show help

```txt
kubectl-view-allocations -h

kubectl-view-allocations 0.8.0
https://github.com/davidB/kubectl-view-allocations
kubectl plugin to list allocations (cpu, memory, gpu,... X requested, limit, allocatable,...)

USAGE:
    kubectl-view-allocations [FLAGS] [OPTIONS]

FLAGS:
    -h, --help         Prints help information
    -z, --show-zero    Show lines with zero requested and zero limit and zero allocatable
    -V, --version      Prints version information

OPTIONS:
    -g, --group-by <group-by>...              Group informations hierarchically (default: -g resource -g node -g pod)
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

 Resource                                   Requested  %Requested  Limit  %Limit  Allocatable  Free
  nvidia.com/gpu                                    7         58%      7     58%           12     5
  ├─ node-gpu1                                      1         50%      1     50%            2     1
  │  └─ xxxx-784dd998f4-zt9dh                       1                  1
  ├─ node-gpu2                                      0          0%      0      0%            2     2
  ├─ node-gpu3                                      0          0%      0      0%            2     2
  ├─ node-gpu4                                      1         50%      1     50%            2     1
  │  └─ aaaa-1571819245-5ql82                       1                  1
  ├─ node-gpu5                                      2        100%      2    100%            2     0
  │  ├─ bbbb-1571738839-dfkhn                       1                  1
  │  └─ bbbb-1571738888-52c4w                       1                  1
  └─ node-gpu6                                      2        100%      2    100%            2     0
     ├─ bbbb-1571738688-vlxng                       1                  1
     └─ cccc-1571745684-7k6bn                       1                  1
```

### overview only

```sh
> kubectl-view-allocations -g resource

 Resource            Requested  %Requested  Limit  %Limit  Allocatable   Free
  cpu                       11          6%     56     28%          200    144
  ephemeral-storage          0          0%      0      0%          5Ti    5Ti
  memory                  25Gi          5%  164Gi     34%        485Gi  320Gi
  nvidia.com/gpu             9         75%      9     75%           12      3
  pods                       0          0%      0      0%          1Ki    1Ki
```

### group by namespaces

```sh
> kubectl-view-allocations -g namespace

 Resource            Requested  %Requested  Limit  %Limit  Allocatable   Free
  cpu                       11          6%     56     28%          200    144
  ├─ default                 2                 28
  └─ dev                     9                 28
  ephemeral-storage          0          0%      0      0%          5Ti    5Ti
  memory                  25Gi          5%  164Gi     34%        485Gi  320Gi
  ├─ cert-manager         96Mi              256Mi
  ├─ default               4Gi               76Gi
  ├─ dev                  13Gi               77Gi
  ├─ dns-external        280Mi              680Mi
  ├─ docs                256Mi              384Mi
  ├─ ingress-nginx       512Mi                2Gi
  ├─ kube-system         640Mi              840Mi
  ├─ loki                  1Gi                1Gi
  ├─ monitoring            3Gi                3Gi
  └─ weave                 1Gi                1Gi
  nvidia.com/gpu             9         75%      9     75%           12      3
  └─ dev                     9                  9
  pods                       0          0%      0      0%          1Ki    1Ki
```

### show as csv

In this case value as expanded as float (with 2 decimal)

```sh
kubectl-view-allocations -o csv
resource,node,pod,Requested,%Requested,Limit,%Limit,Allocatable,Free
cpu,,,59.84,22%,104.10,38%,272.00,167.90
cpu,node-gpu1,,2.31,19%,4.47,37%,12.00,7.53
cpu,node-gpu1,zzz-b8bd56fbd-5x8vq,1.00,,2.00,,,
cpu,node-gpu1,kube-flannel-ds-amd64-7dz9z,0.10,,0.10,,,
cpu,node-gpu1,node-exporter-gpu-b4w7s,0.11,,0.22,,,
cpu,node-gpu1,xxx-backend-7d84544458-46qnh,1.00,,2.00,,,
cpu,node-gpu1,weave-scope-agent-bbdnz,0.10,,0.15,,,
cpu,node-gpu2,,0.31,1%,0.47,2%,24.00,23.53
cpu,node-gpu2,kube-flannel-ds-amd64-b5b4v,0.10,,0.10,,,
cpu,node-gpu2,node-exporter-gpu-796jz,0.11,,0.22,,,
cpu,node-gpu2,weave-scope-agent-8rhnd,0.10,,0.15,,,
cpu,node-gpu3,,3.41,11%,6.67,21%,32.00,25.33
...
```

It can be combined with "group-by" options.

```sh
kubectl-view-allocations -g resource -o csv
resource,Requested,%Requested,Limit,%Limit,Allocatable,Free
cpu,59.84,22%,104.10,38%,272.00,167.90
ephemeral-storage,0.00,0%,0.00,0%,34462898618662.00,34462898618662.00
hugepages-1Gi,0.00,,0.00,,,
hugepages-2Mi,0.00,,0.00,,,
memory,67989667840.00,9%,216094736384.00,30%,722318667776.00,506223931392.00
nvidia.com/gpu,2.00,18%,2.00,18%,11.00,9.00
pods,0.00,0%,0.00,0%,1540.00,1540.00
```

## Alternatives

- see the discussion [Need simple kubectl command to see cluster resource usage · Issue #17512 · kubernetes/kubernetes](https://github.com/kubernetes/kubernetes/issues/17512)
