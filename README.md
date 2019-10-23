# kubectl-view-allocations

[![Crates.io](https://img.shields.io/crates/l/kubectl-view-allocations.svg)](http://creativecommons.org/publicdomain/zero/1.0/)
[![Crates.io](https://img.shields.io/crates/v/kubectl-view-allocations.svg)](https://crates.io/crates/kubectl-view-allocations)

[![Project Status: WIP – Initial development is in progress, but there has not yet been a stable, usable release suitable for the public.](https://www.repostatus.org/badges/latest/wip.svg)](https://www.repostatus.org/#wip)
[![Actions Status](https://github.com/davidB/kubectl-view-allocations/workflows/ci-flow/badge.svg)](https://github.com/davidB/kubectl-view-allocations/actions)
[![Documentation](https://docs.rs/kubectl-view-allocations/badge.svg)](https://docs.rs/kubectl-view-allocations/)

kubectl plugin to list allocations (cpu, memory, gpu,... X requested, limit, allocatable,...).

## Install

### via cargo

```sh
cargo install kubectl-view-allocation
```

## Usage

### Show help

```txt
kubectl-view-allocations -h

kubectl-view-allocations 0.1.0-dev
https://github.com/davidB/kubectl-view-allocations
kubectl plugin to list allocations (cpu, memory, gpu,... X requested, limit, allocatable,...)

USAGE:
    kubectl-view-allocations [FLAGS] [OPTIONS]

FLAGS:
    -h, --help         Prints help information
    -z, --show-zero    Show lines with zero requested and zero limit and zero allocatable
    -V, --version      Prints version information

OPTIONS:
    -n, --namespace <namespace>               Show only pods from this namespace
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
  │  └─ bbbb-atamborrino-1571738888-52c4w           1                  1
  └─ sail-gpu6                                      2        100%      2    100%            2     0
     ├─ bbbb-1571738688-vlxng                       1                  1
     └─ cccc-1571745684-7k6bn                       1                  1
```
