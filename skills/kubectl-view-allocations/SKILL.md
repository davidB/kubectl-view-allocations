---
name: kubectl-view-allocations
description: "Use when inspecting Kubernetes resource allocation with kubectl-view_allocations. For allocation questions, use kubectl-view_allocations first and do not replace it with native kubectl get/describe/top/jsonpath queries. Covers cluster/node/namespace/pod requests, limits, allocatable, free capacity, GPU or custom resources, taint-filtered node views, CSV exports, metrics-server utilization via kubectl-view_allocations -u, kubeconfig/context access checks, and allocation-focused troubleshooting. Trigger for CPU, memory, GPU, pod capacity, overcommit, missing requests/limits, tainted nodes, or comparing requested/limit/utilization data in a Kubernetes cluster."
---

# Kubectl View Allocations

Use `kubectl-view_allocations` as the required first tool for this repository's allocation workflow. Do not answer allocation questions by reconstructing the same data with native `kubectl get`, `kubectl describe`, `kubectl top`, JSONPath, or ad hoc scripts unless `kubectl-view_allocations` cannot run and the user accepts a fallback.

`kubectl-view_allocations` reports Kubernetes allocations from manifests and node allocatable resources; it is not a replacement for `kubectl top`, except when `-u/--utilization` is explicitly requested and Metrics API is available.

## Workflow

1. Clarify scope only when it is missing and risky: kube context, namespace, resource name, node selector, or whether tainted nodes should be included.
2. Run the first data-gathering query with `kubectl-view_allocations`.
3. Run a narrow `kubectl-view_allocations` query first, then broaden only if the result looks incomplete.
4. Prefer allocation views for capacity planning and scheduling questions; use `kubectl-view_allocations -u` only for "currently using" questions.
5. Explain output in terms of `Requested`, `Limit`, `Allocatable`, and `Free`.
6. If `kubectl-view_allocations` fails, fix the command invocation path first: installation/PATH, kube context, kubeconfig, permissions, metrics availability, or `--precheck`.

## Tool Policy

- Use `kubectl-view_allocations` for allocation facts and summaries.
- Use `kubectl` only for narrow support tasks: checking the current context, refreshing auth through `kubectl-view_allocations --precheck`/`kubectl cluster-info`, confirming metrics-server availability after `kubectl-view_allocations -u` fails, or fetching metadata that `kubectl-view_allocations` cannot show and the user explicitly needs.
- Do not start with `kubectl top` for utilization questions covered by `kubectl-view_allocations -u`.
- Do not start with `kubectl get nodes/pods -o json` to manually sum requests, limits, allocatable resources, or taints.
- If `kubectl-view_allocations` is missing, guide the user to install it from the davidB/kubectl-view-allocations README before using fallback allocation commands.
- If falling back from `kubectl-view_allocations` is unavoidable, tell the user that the fallback is no longer using this skill's primary tool and why.

## Missing Tool

When `kubectl-view_allocations` is not installed or not on PATH:

1. Tell the user the primary tool is missing.
2. Offer the README-supported install options and ask which they prefer:
   - krew: `kubectl krew install view-allocations`
   - cargo: `cargo install kubectl-view-allocations`
   - release script: `curl https://raw.githubusercontent.com/davidB/kubectl-view-allocations/master/scripts/getLatest.sh | bash`
3. After installation, run `kubectl-view_allocations --help` or a narrow allocation query to confirm it works.
4. Do not silently replace the missing tool with native `kubectl` allocation calculations.

## Command Patterns

Use these starting points:

```sh
kubectl-view_allocations -g resource
kubectl-view_allocations -g resource,node
kubectl-view_allocations -g resource,node,pod
kubectl-view_allocations -g namespace
kubectl-view_allocations -n NAMESPACE -g resource,node,pod
kubectl-view_allocations -r cpu,memory -g resource,namespace
kubectl-view_allocations -r gpu
kubectl-view_allocations --ignore-taints -g resource,node
kubectl-view_allocations -u -r cpu,memory
kubectl-view_allocations -o csv -g resource,node,pod
```

Add `--context CONTEXT` and `--kubeconfig PATH` when the user names a cluster or config file. Use `--precheck` when kube auth tokens may need refreshing because it runs `kubectl cluster-info` first.

Read `references/command-cookbook.md` for command recipes, interpretation notes, and troubleshooting branches.

## Interpretation

- `Requested`: sum of pod container resource requests from manifests.
- `Limit`: sum of pod container limits from manifests.
- `Allocatable`: node allocatable capacity.
- `Free`: `Allocatable - max(Limit, Requested)` by default; use `--used-mode only-request` to compute free capacity from requests only.
- `Utilization`: live CPU/memory usage from Metrics API, shown only with `-u`.
- `__`: value is absent or not meaningful at that grouping level, not necessarily zero.

Default grouping is `resource,node,pod`; `resource` is always prepended when omitted. Default node filtering excludes tainted nodes. Use `--ignore-taints` with no value to include all nodes, or with taint keys/key-value pairs to include specific tainted nodes.

## Safety

Treat `kubectl-view_allocations` as read-only. It lists nodes, pods, and optionally pod metrics. Do not deploy, delete, scale, or modify Kubernetes resources as part of this skill unless the user separately asks for that action.
