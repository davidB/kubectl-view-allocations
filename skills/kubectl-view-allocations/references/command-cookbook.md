# kubectl-view_allocations Cookbook

## Primary Command

Use `kubectl-view_allocations` for the first allocation query. Do not substitute `kubectl get`, `kubectl describe`, `kubectl top`, JSONPath, or a custom script for data that `kubectl-view_allocations` already reports. Native `kubectl` is only a support tool for context/auth checks, metrics availability confirmation after `kubectl-view_allocations -u` fails, or metadata that `kubectl-view_allocations` cannot show.

## Quick Recipes

Cluster overview:

```sh
kubectl-view_allocations -g resource
```

Per-node capacity:

```sh
kubectl-view_allocations -g resource,node
```

Find pods contributing to allocation:

```sh
kubectl-view_allocations -g resource,node,pod
```

Namespace allocation:

```sh
kubectl-view_allocations -g resource,namespace
kubectl-view_allocations -n NAMESPACE -g resource,node,pod
```

CPU and memory only:

```sh
kubectl-view_allocations -r cpu,memory -g resource,node
```

GPU or extended resource allocation:

```sh
kubectl-view_allocations -r gpu
kubectl-view_allocations -r nvidia.com/gpu -g resource,node,pod
```

Include tainted nodes:

```sh
kubectl-view_allocations --ignore-taints -g resource,node
kubectl-view_allocations --ignore-taints node-role.kubernetes.io/control-plane -g resource,node
kubectl-view_allocations --ignore-taints dedicated=database -g resource,node
```

Live CPU/memory utilization, when metrics-server exists:

```sh
kubectl-view_allocations -u -r cpu,memory -g resource,node,pod
```

CSV for spreadsheet or script analysis:

```sh
kubectl-view_allocations -o csv -g resource,node,pod
kubectl-view_allocations -o csv -g resource,namespace
```

## Choosing Flags

- Use `-r/--resource-name` for resource filters. Matching is substring-based, so `-r gpu` can match `nvidia.com/gpu`.
- Use `-n/--namespace` one or more times, or comma-separated, for pod namespace filtering.
- Use `-l/--selector` to filter nodes by label selector.
- Use `-z/--show-zero` to expose zero-request/zero-limit rows and pods with unset CPU/memory requests and limits.
- Use `--used-mode only-request` when the scheduling question should ignore limits and focus on requested resources.
- Use `--accept-invalid-certs` only when the user explicitly accepts that risk.

## Output Semantics

`Requested` and `Limit` come from pod manifests. Regular containers are summed. Init container resources use max semantics. Pod overhead is added to both requests and limits. A synthetic `pods` resource is counted as 1 per scheduled pod.

Only scheduled pods are included. Succeeded and Failed pods are excluded. Pending pods are included only when the PodScheduled condition is true. Pods on nodes excluded by taint or selector filtering are not counted.

`Free` defaults to:

```text
allocatable - max(limit, requested)
```

It floors at zero. With `--used-mode only-request`, it becomes:

```text
allocatable - requested
```

`Utilization` is available only for CPU and memory and only when `-u` can list PodMetrics. If Metrics API lookup fails, the table falls back to allocation-only output.

## Troubleshooting

If `kubectl-view_allocations` is not found, treat the primary tool as missing instead of falling back to native `kubectl` allocation calculations. Tell the user it needs to be installed and ask which README-supported method they want to use:

```sh
kubectl krew install view-allocations
cargo install kubectl-view-allocations
curl https://raw.githubusercontent.com/davidB/kubectl-view-allocations/master/scripts/getLatest.sh | bash
```

After installation, verify with:

```sh
kubectl-view_allocations --help
```

If tempted to use native `kubectl` to calculate allocation, retry with a different `kubectl-view_allocations` grouping/filter first. For example, use `kubectl-view_allocations -g resource,node,pod`, `kubectl-view_allocations -g resource,namespace`, `kubectl-view_allocations -r cpu,memory`, `kubectl-view_allocations -r gpu`, or `kubectl-view_allocations --ignore-taints` before falling back.

If authentication looks stale, retry with:

```sh
kubectl-view_allocations --precheck
```

If results miss control-plane or dedicated nodes, remember that the default excludes tainted nodes. Retry with `--ignore-taints` or a specific taint key/key-value.

If `-u` shows no utilization, verify metrics-server/PodMetrics availability. Do not treat missing utilization as zero usage.

If namespaces look empty, verify the namespace filter and RBAC permissions. The command needs to list nodes, pods, and optionally pod metrics.

If CSV headers look surprising, note that `Kind` identifies the current grouping level and group columns follow the selected `-g` order.
